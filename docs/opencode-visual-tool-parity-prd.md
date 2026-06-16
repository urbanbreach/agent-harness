# Agent Harness PRD: OpenCode 1:1 Visual & Tool-Presentation Parity (first public release)

**Status:** Implementation PRD. Produced from a guided source + screenshot
comparison of OpenCode against the Harness TUI on 2026-06-15. No source files
were modified while producing this document.

**Audience:** A future implementation agent (or agents) executing this plan in
small, reviewable phases.

**Authority:** Subordinate to [`docs/roadmap-v1.md`](roadmap-v1.md) for product
scope and to the root [`AGENTS.md`](../AGENTS.md) invariants for runtime
architecture. **Complementary to**
[`docs/agent_harness_opencode_ui_pi_backend_prd.md`](agent_harness_opencode_ui_pi_backend_prd.md)
("the prior PRD"): that document owns OpenCode **interaction vocabulary** and
**dialog/permission depth** (leader keymap, composer editing, shell mode, stash,
session-list management, model dialogs, permission-modal staging). This document
owns **visual fidelity**, **tool-call presentation**, the **two new backend
features** (working-tree revert, auto-format), and the **scope reconciliation**
needed for a vanilla-OpenCode first public release. Where the two PRDs touch the
same surface (theme, footer, sidebar), this PRD supplies the concrete
OpenCode-matched *visual* target; the prior PRD remains authoritative for the
*behavior* of those surfaces. Implementation must not weaken anything either
upstream document guarantees.

> **STATUS — VISUAL PARITY REVERTED (2026-06-16).** The visual TUI changes
> applied for this PRD degraded the existing Harness TUI compared to its pre-PRD
> state and to the OpenCode reference. All harness-tui source and snapshot files
> have been reverted to commit `3ffaf9a5`. The backend features
> (working-tree revert, auto-format) remain implemented and are preserved.
> Every visual/ui task (P2, P3, P4, P6-2) must be redone, this time using the
> actual OpenCode source under `inspirations/opencode/` as the reference while
> preserving the Harness TUI's visual identity.

---

## 0. Read this first

### 0.1 Governing goal

> Make the Harness TUI a 1:1 visual copy of OpenCode's local-coding terminal UI,
> reimplemented natively in Rust/Ratatui, and make tool calls render exactly as
> OpenCode renders them — so that on the first public release Harness *looks and
> reads* like vanilla OpenCode while keeping its Pi-inspired, event-sourced
> backend. Add working-tree revert and auto-format-on-edit as the two remaining
> vanilla-OpenCode core features worth shipping for v1.

This is the design language for **all** future UI work: any new surface, whether
it comes from OpenCode or is Harness-original, must be built in the OpenCode
visual idiom defined here unless this PRD or a successor explicitly re-scopes it.

The work breaks into eight workstreams plus a baseline/evidence gate:

- **WS-A — Theme role remap.** Repoint Harness theme roles onto OpenCode's
  palette (the values already exist in `theme.rs`; the *roles* are mis-assigned).
- **WS-B — Start screen parity.**
- **WS-C — Session transcript layout parity** (the highest-traffic surface).
- **WS-D — Tool-call presentation parity.**
- **WS-E — Command palette parity.**
- **WS-F — Working-tree revert/undo** (new backend feature, replay-safe).
- **WS-G — Auto-format on edit** (new backend feature).
- **WS-H — Scope reconciliation, parity spec doc, evidence.**

This is **not** a rewrite, **not** a new UI framework, **not** a re-litigation
of which agents/tools exist (see §3 decisions). It is presentation fidelity plus
two scoped features.

### 0.2 Implementation-agent operating rules

1. Read the root `AGENTS.md` and the crate-scoped `AGENTS.md` for every crate you
   touch **before** the first edit of a phase.
2. Load the mandatory coding skill `karpathy-guidelines` before the first code
   edit, per root `AGENTS.md`. Delegated coding tasks must include it in
   `load_skills`.
3. Work phase by phase (§7). Do not start phase N+1 while phase N acceptance
   criteria are unmet, except for tasks marked explicitly independent.
4. **For every task — visual, tool, backend, or docs — re-open the cited
   inspiration reference file(s) in `inspirations/` before the first edit, and
   survey adjacent files in the same directory so you understand the behavior in
   context, not in isolation.** Do not implement from memory or from this
   document alone; this document tells you *where to look* and *what the target
   is*, the reference is ground truth. **Additionally, for every visual/tool
   task:** also open the matching parity screenshot before implementing and
   re-capture a Harness screenshot/snapshot at matching geometry after. If a card
   names no reference file, that is a defect — find the relevant `inspirations/`
   source (OpenCode first; Pi/OMO/Codex where the card's philosophy points there)
   and record what you used on the Evidence line.
5. Honor the UPDATE-TOGETHER table in root `AGENTS.md`: config keys, event
   variants, tool ids, theme tokens, and test-lane behavior changes update their
   paired docs/schemas/tests in the same change.
6. Use the narrowest test lane that proves a change (`scripts/test-lanes.sh fast`
   first; see `docs/testing.md`).
7. Snapshot updates go through `cargo insta review` only after deciding whether
   behavior changed or the fixture drifted.
8. You retain engineering judgment on *how* to implement each card. Where this
   PRD names files, treat them as the likely seam, not a mandate — if a cleaner
   seam exists, use it and note the deviation in the task's evidence line.

### 0.3 Checkbox protocol (required)

This PRD uses `- [ ]` checkboxes for every task card (§8) and every final
acceptance criterion (§9).

- A box may be checked **only after** its listed verification command(s) have
  been run and passed, and the result recorded. Checking a box is a claim that
  evidence exists.
- When you check a box, append the evidence inline on the task's **Evidence:**
  line: the exact command(s) run and a one-line result (e.g.
  `cargo test -p harness-tui transcript_user_message -- --nocapture → ok, 12 passed`),
  plus the artifact path for any re-captured screenshot/snapshot.
- **Never** check a box you have not verified. **Never** uncheck-and-skip a box
  to make a phase look complete. A task that is deferred is marked
  `- [~]` with a one-line written disposition, not left silently unchecked.
- A phase is complete only when every P0/P1 box in it is `- [x]` with evidence
  or `- [~]` with disposition.

### 0.4 Non-negotiable invariants (verbatim from the tree)

- Events are the source of truth; replay is side-effect free and derives from
  JSONL in contiguous `seq` order.
- The coordinator (`harness-core::coord`) is the only event-append, scheduling,
  permission, hook, compaction, and lifecycle authority. The TUI never appends
  events, resolves permissions locally, or executes tools.
- Permission checks precede tool execution. Hashline edits validate anchors,
  reject overlaps, apply bottom-up, write atomically, and stay the normal
  file-changing path.
- Replay mode in the TUI is read-only and must not emit live submission intents.
- Provider/tool metadata persisted to events/artifacts must be redacted: never
  store raw requests/responses, auth headers, cookies, keys, PEM blocks, or
  hidden reasoning text.
- Runtime config (`harness.json{,c}`) and TUI config (`tui.json{,c}`) are
  separate public contracts; new theme/visual settings belong to `tui.json{,c}`.
- `inspirations/` is read-only reference material; copy observable behavior and
  visual design, never source code, package layout, or branding.

### 0.5 Anti-gaming rules

- Do not weaken, delete, `#[ignore]`, assert-loosen, or rubber-stamp any test or
  snapshot to reach green. Snapshot churn is resolved by a behavior-vs-fixture
  verdict recorded in the commit, then `cargo insta review`.
- Do not check a §8 task box or a §9 acceptance box without running its listed
  verification and recording the result on the Evidence line (§0.3).
- Do not declare a visual parity task done by building a "vaguely similar"
  surface. Differences from OpenCode are allowed **only** when recorded in the
  task's **Adaptation:** note and justified by a §0.4 invariant, Harness
  architecture, or an explicit §4 non-goal (e.g. excluded share/cloud regions).
- For revert (WS-F): do not introduce any replay-time side effect. Restore is a
  live operator action only; prove it with a replay-absence test.
- Do not silently expand scope into §4 non-goals.

### 0.6 Definition of done

This PRD is complete iff, simultaneously on the working branch:

1. All P0 and P1 task boxes in §8 are `- [x]` with evidence, or `- [~]` with a
   written disposition appended to §10. P2 boxes may be deferred but must be
   dispositioned in writing.
2. `scripts/test-lanes.sh fast`, `quality-gates`, and `all-deterministic` pass
   with zero failures.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
4. The §9 final acceptance boxes hold, including the §9 side-by-side screenshot
   comparison for each parity surface at matching geometry.
5. Docs named in §8 (WS-H) are updated together with the code that changed them.
6. A dogfooding evidence note is recorded in §10.

### 0.7 Parity bar (scope: the surfaces named in §5/§6)

For each selected surface, the bar is **1:1 with OpenCode's local-coding TUI**:
same screen composition, information hierarchy, spacing rhythm, glyph language,
and color *roles* — rendered with Harness content and Harness theme tokens.
Excluded OpenCode regions (cloud/share, `/connect`, plugins, snapshot-based undo
UI we are not adopting, multimodal paste) are out and their footer/dialog space
is filled with Harness equivalents or left empty. When the reference conflicts
with a §0.4 invariant, Harness wins and the deviation is recorded as an
**Adaptation:** note on the task.

---

## 1. Executive summary

The Harness TUI is already close to OpenCode in skeleton — two-column session
shell, sidebar with Context/MCP/Changes, command palette, permission modal, diff
machinery — and on the **tool surface** Harness is at or beyond OpenCode (it adds
ast-grep, session tools, background tasks, batch, codesearch, github — mostly
native ports of oh-my-openagent ideas). The remaining distance to "looks like
vanilla OpenCode" is almost entirely **presentation**:

- **Color roles are wrong, not the palette.** `theme.rs` already encodes
  OpenCode's exact values (`#eeeeee`, `#fab283`, `#ffc09f`, `#9d7cd8`,
  `#56b6c2`, `#f5a742`), but Harness wires **orange `#f5a742`** into the prompt
  border, selection bar, and logo where OpenCode uses **peach/salmon/purple/teal**.
- **Start screen** duplicates the model/launch metadata above and inside the
  input box, lacks a bottom status bar, lacks the mcp/agents/tip hint cluster,
  and uses a thin spaced `HARNESS` wordmark instead of OpenCode's two-tone block
  wordmark treatment.
- **Session transcript** boxes user messages in a heavy accent block (OpenCode
  flows them as plain text), uses a tall padded prompt input with an embedded
  `Session:` line (OpenCode uses a slim input with the agent/model line above),
  shows an opaque `run_<id>` instead of a generated session title, injects
  per-turn `• agent · model · 2.2s` chrome OpenCode does not, and has a markdown
  rendering bug that fuses words with stray colored backgrounds.
- **Tool calls** render the **raw tool id** as the title (`read`,
  `edit.apply_patch`) instead of OpenCode's verb form (`Read <path>`,
  `Patched <path>`), and `apply_patch` dumps the raw `*** Begin Patch ***`
  payload plus an artifact link instead of rendering an inline red/green
  line-numbered diff — even though the diff machinery already exists.
- **Command palette** uses gray headers + orange selection + command-id words
  where OpenCode uses purple headers + salmon selection + keybinding hints, and
  ships far fewer commands.

Two vanilla-OpenCode backend features remain worth shipping for v1:
**working-tree revert/undo** of agent edits (OpenCode's snapshot/`revert.ts`),
and **auto-format on edit** (OpenCode's `format/`).

Everything else from this conversation is a **scope decision** captured in §3.

---

## 2. Evidence log

### 2.1 Comparison performed (read-only)

OpenCode source (`inspirations/opencode/packages/opencode/src/`):
- TUI component tree under `cli/cmd/tui/` — `routes/home.tsx`,
  `routes/session/index.tsx`, `routes/session/footer.tsx`,
  `routes/session/sidebar.tsx`, `component/command-palette.tsx`,
  `component/prompt/index.tsx`, `feature-plugins/sidebar/*`,
  `feature-plugins/system/diff-viewer*`, `util/transcript.ts`,
  `util/collapse-tool-output.ts`, `util/revert-diff.ts`,
  `context/theme/opencode.json` (default palette).
- `agent/agent.ts` built-in agents (build, plan primary; general, explore
  subagent) and `tool/` set (read, write, edit, apply_patch, glob, grep, lsp,
  shell, task, skill, question, todo, webfetch, websearch, plan, external-directory).
- `session/revert.ts` (snapshot track/restore) and `format/index.ts`
  (formatter-after-write) as the references for the two new features.
- Per-model-family prompts: `session/prompt/{anthropic,gpt,gemini,codex,
  copilot-gpt-5,kimi,beast,default}.txt` — confirms OpenCode ships per-family
  prompts (relevant to §3 decision 2).

Harness source:
- `crates/harness-tui/src/` — `theme.rs` (palette values already present;
  role mapping at the `agent_accent`/theme-variant block), `ui_tool_titles.rs`
  (`generic_tool_name` returns the raw `tool_id`), `ui_tool_diffs.rs`
  (`tool_call_inline_diff_block`, `collect_apply_patch_file_render_entries` —
  inline-diff machinery exists), `ui_transcript*`, `ui_secondary/*` (sidebar),
  `ui_chrome.rs` (footer), `ui_markdown.rs` (markdown rendering).
- `crates/harness-tools/src/` — tool set (`docs/native-tool-catalog.md`),
  hashline create-path in `hashline_edit.rs` (anchorless append creates files;
  confirms no standalone `write` is needed).

### 2.2 Screenshots used (ground truth)

`inspirations/screenshots opencode ui parity/Opencode/` (start screen, chat
examples 1–5, CTRL+P commands menu, slash-command menu) and
`.../Harness project/` (current start screen, chat example 1, CTRL+P commands
menu, `live_proxy_*` tool-flow captures). Chat example 4 (OpenCode) is the
reference for inline diff rendering; the `live_proxy_edit_apply_patch_finished`
capture shows the current raw-payload rendering to be replaced.

### 2.3 Key code facts

| Fact | Evidence |
|---|---|
| Palette values present, roles mis-assigned | `theme.rs` variant block (`primary #EEEEEE`, `accent #F5A742` and `#9D7CD8`); `opencode.json` defs |
| Tool titles are raw ids | `ui_tool_titles.rs` `generic_tool_name(tool_id) = tool_id.trim().to_string()` |
| Inline-diff machinery exists but apply_patch shows raw payload | `ui_tool_diffs.rs` has `tool_call_inline_diff_block`; `live_proxy_edit_apply_patch_finished.png` shows `*** Begin Patch ***` + artifact link |
| Sidebar already has Context/MCP/Changes | `live_proxy_*` captures; `ui_secondary/sidebar_sections.rs` |
| No standalone `write` tool; hashline append creates files | `docs/native-tool-catalog.md`; `hashline_edit.rs` |
| No working-tree revert; no format-on-edit | negative grep in `crates/` (only Formatter trait impls, compaction checkpoints, session-state snapshots) |

---

## 3. Scope decisions (settled this cycle — do not re-litigate)

1. **Category routes stay.** `visual-engineering`, `artistry`, `ultrabrain`,
   `deep`, `quick`, `unspecified-low/high`, `writing` and `task(category=...)`
   are retained. They are lightweight subagent profiles, not extra main agents,
   and are cheap to maintain. **Not** an overreach; **not** in scope to remove.
2. **Per-model-family prompts stay — they are genuine OpenCode parity, not
   orchestration overreach.** OpenCode ships per-family prompt files; Harness's
   family-resolution seam matches it. WS-H includes a one-line doc correction
   where the roadmap implies otherwise.
3. **No standalone `write` tool.** File creation goes through the hashline edit
   path (anchorless append), which is the more effective editing model. Do not
   add a `write` tool. (If a future OpenCode-trained prompt reaches for `write`,
   that is handled in prompt text, not by adding the tool.)
4. **Harness's extra tools stay, as first-class native implementations.**
   ast-grep (search/replace), `session_*`, `background_*`, `batch`,
   `codesearch`, `github.*`, `lsp.rename` remain. Most originate from
   oh-my-openagent and are kept as native Rust ports. They are not parity
   blockers and are not to be trimmed for "vanilla" appearance.
5. **Custom markdown slash commands (`$ARGUMENTS`) remain intentionally out of
   v1**, per the existing roadmap (replay-safety cost). Not in this PRD.
6. **Backend philosophy stays Pi-inspired**; this PRD adds revert/format as
   coordinator-owned, replay-safe features in that idiom.

---

## 4. Non-goals and scope guards

- No new UI framework; all work uses the existing Ratatui theme-token and
  layout-contract systems. No source-ported components.
- No second theme engine. WS-A reuses the existing `Theme` token families and,
  where a user-selectable theme is needed, a `tui.json` key (shared with the
  prior PRD's T-UI-09).
- No moving coordinator authority out of `harness-core`. Revert and format are
  coordinator-owned; the TUI only sends intents.
- No making replay effectful. Revert restore is a live operator action; it must
  be unavailable or read-only in replay mode.
- No cloud/share/account/plugin surfaces, even though OpenCode ships them;
  affected regions get Harness equivalents or are left empty.
- No agent-set or tool-set changes beyond §3 (no removing categories, no adding
  `write`, no removing extras).
- No custom markdown commands (§3.5).
- No interaction-vocabulary work that the prior PRD already owns (leader keymap,
  composer editing, shell mode, stash, session-list management, model dialogs,
  permission staging) — this PRD only sets the *visual* target where they
  overlap.

---

## 5. Visual parity findings & targets — TUI chrome

Each row: reference → current Harness → target → primary files. Re-open the
reference file + screenshot before implementing (rule §0.2.4).

### V-A. Theme role remap [WS-A]
- Reference: `context/theme/opencode.json`. Roles: `primary = #fab283` (peach;
  prompt border, focused accents), selection bar `#ffc09f` (salmon), section
  headers / `accent = #9d7cd8` (purple), live/secondary accent `#56b6c2`
  (teal), text `#eeeeee`, muted `#808080`, step backgrounds `#0a0a0a…#3c3c3c`.
- Current: orange `#f5a742` is wired into prompt border, palette selection, and
  logo highlight — overriding the peach/salmon/purple roles.
- Target: remap the Harness theme roles onto the OpenCode tokens (values mostly
  already in `theme.rs`). Keep orange only where OpenCode uses orange
  (`darkOrange #f5a742` is OpenCode's *warning*, not its primary). One change
  repaints most surfaces ~half the way to parity.
- Files: `crates/harness-tui/src/theme.rs` (+ any hardcoded `Color::Rgb` call
  sites that bypass theme tokens — grep and route them through tokens).

### V-B. Start screen [WS-B]
- Reference: `routes/home.tsx` + `Opencode start screen.png`.
- Gaps: (1) two-tone block wordmark vs thin spaced `HARNESS`; (2) model/launch
  metadata duplicated above *and* inside the input box — show once, inside the
  box, second line; (3) missing bottom status bar (`~/cwd  ⊙ N MCP  /status … version`);
  (4) missing hint cluster (`● N mcp servers` green dot, `tab agents`,
  `ctrl+p commands`) and the `● Tip …` line.
- Target: match composition 1:1 — wordmark treatment, single metadata line in
  box, bottom status bar, hint cluster, tip line. Harness branding/copy.
- Files: home/startup render in `ui.rs` / `ui_composer.rs` / `ui_chrome.rs`,
  `theme.rs` (wordmark + copy tokens).

### V-C. Session transcript layout [WS-C]
- Reference: `routes/session/index.tsx`, `routes/session/sidebar.tsx`,
  `routes/session/footer.tsx`, chat examples 1–4.
- Gaps:
  - **User messages boxed.** Harness wraps each user turn in a heavy full-width
    accent box; OpenCode flows user text as plain text with a subtle marker.
    → unbox.
  - **Prompt input heavy.** Tall padded box with embedded `Session:` line →
    slim 1–2 line input with the agent/model line *above* it (the prior PRD's
    composer behavior is unaffected; this is geometry/decoration only).
  - **Footer structure.** Current = status/error text + `Enter send q quit`.
    Target = `agent (Role) · model · variant` left, `tokens (%) · $cost ·
    ctrl+p` right; status banner on its own line above only when present
    (coordinate with prior PRD T-UI-01).
  - **Sidebar title** = generated session title (not `run_<id>`). Verify/finish
    LSP section, per-server MCP status dots, and Modified-Files `+N -M`
    diffstat to match OpenCode; add the `• Harness <version>` brand footer line
    (coordinate with prior PRD T-UI-08a).
  - **Todos** rendered as an inline `# Todos` checkbox block atop the transcript
    (verify current placement; align to reference).
  - **Thinking** rendered as inline italic `Thinking:` blocks.
- Files: `ui_transcript*`, `ui_transcript_render.rs`, `ui_secondary/*`,
  `ui_chrome.rs`, `ui_markdown.rs`.

### V-D. Command palette [WS-E]
- Reference: `component/command-palette.tsx` + `CTRL+P commands menu.png`.
- Gaps: gray headers → **purple** section headers; orange selection → **salmon**
  full-width bar; right column shows a command-id word → show the **keybinding**
  (`ctrl+x l`); drop inline descriptions (OpenCode shows name + keybind only);
  fill out the command set toward OpenCode's (Switch model, Open editor, Skills,
  View status, Switch theme, Help, Open docs, New/Continue/Replay session) —
  only for commands Harness actually has; do not invent commands for absent
  features.
- Files: `ui_overlays/*` (palette), `keybindings/command_registry.rs`,
  `theme.rs`.

---

## 6. Tool-call presentation findings & targets [WS-D]

Reference: `routes/session/index.tsx` (tool part rendering, `↳` result lines),
`feature-plugins/system/diff-viewer*`, chat example 4; Harness
`live_proxy_*` captures.

### T-A. Verb-form tool titles
- Current: `generic_tool_name` returns the raw `tool_id` → transcript shows
  `read`, `edit.apply_patch`, `grep`, `bash`.
- Target: a tool-id → display-verb map producing OpenCode's forms: `Read <path>`,
  `Wrote <path>` / `Patched <path>` (edit/apply_patch), `Grep "<pattern>"`,
  `List <dir>`, `Searched <query>`, `Fetched <url>`, `Ran <cmd>` (bash), plus
  sensible verbs for the Harness extras (`Searched (ast-grep) …`,
  `Read session …`, etc.). Unknown/MCP tools fall back to a titlecased name.
  Keep the `↳ <result>` continuation line — that already matches OpenCode.
- Files: `ui_tool_titles.rs` (replace `generic_tool_name`), title tests.

### T-B. Inline diff for edit/apply_patch
- Current: `edit.apply_patch` echoes the raw `*** Begin Patch ***` payload as
  the tool input and links a diff artifact; no inline diff.
- Target: render an inline red/green, line-numbered diff in the transcript for
  edit and apply_patch (side-by-side at wide widths, unified when narrow),
  matching OpenCode chat example 4. The machinery exists
  (`ui_tool_diffs.rs::tool_call_inline_diff_block`,
  `collect_apply_patch_file_render_entries`); determine why it does not fire for
  apply_patch in the live flow (likely the diff is only available post-approval,
  or disclosure-gated) and wire it. Do not show the raw patch payload as the
  primary representation; keep the artifact link as secondary/disclosure.
- Files: `ui_tool_diffs.rs`, `ui_transcript_tool_render.rs`, `ui_diff_render.rs`.

### T-C. Markdown rendering bug
- Current: assistant text fuses adjacent words with stray colored backgrounds
  (e.g. `gammaLIVETOOLFLOWREADCONFIRMED`).
- Target: fix the inline markdown/keyword styling so prose renders cleanly with
  correct word boundaries. Reproduce from the `live_proxy` content, add a
  regression test.
- Files: `ui_markdown.rs` (+ `ui_fenced_text.rs` / `ui_syntax_highlight.rs` if
  the highlighter is the source).

### T-D. Trim per-turn chrome
- Current: Harness injects `• <agent> · gpt-5.4-mini · 2.2s` bullet lines
  between steps.
- Target: keep the transcript clean per OpenCode — agent/model lives in the
  footer (V-C). Move per-turn timing into the footer/sidebar or a disclosure,
  not inline between every step. Marker glyphs align to OpenCode (`▸`/`#` family)
  where they remain.
- Files: `ui_transcript_render.rs`, `ui_transcript_sections.rs`, `theme.rs`.

---

## 7. Implementation roadmap

Order rationale: cheap honest-baseline work first; then the single highest-
leverage lowest-risk change (theme); then the highest-traffic surface
(transcript + tools); then start screen + palette; then the two backend features
(parallelizable); then regression + screenshot evidence.

- **Phase 1 — Scope reconciliation & parity spec (WS-H).** Write
  `docs/opencode-tui-parity.md` (the durable "all future UI matches OpenCode"
  reference + per-surface checklist), and land the doc corrections from §3
  (family-prompt parity wording; categories/extras retained; no `write`). No
  runtime code. *Independent; unblocks honest claims.*
- **Phase 2 — Theme role remap (WS-A / V-A).** One reviewed snapshot pass.
- **Phase 3 — Transcript + tool presentation (WS-C, WS-D / V-C, T-A..T-D).**
  Highest traffic; depends on Phase 2 for colors.
- **Phase 4 — Start screen + command palette (WS-B, WS-E / V-B, V-D).**
- **Phase 5 — Backend features (WS-F revert, WS-G format).** Parallelizable from
  Phase 1; sequenced here for review focus.
- **Phase 6 — Regression, screenshots, evidence, dispositions.**

---

## 8. Task backlog

> Priority: P0 = blocks honest claims; P1 = core value; P2 = defer-able with
> written disposition. Check boxes only per §0.3.

### Phase 1 — Scope reconciliation & parity spec

- [x] **P1-1 · Write `docs/opencode-tui-parity.md` · P1 · docs**
  - Walk OpenCode's `routes/` + `component/` tree; per surface record
    *OpenCode behavior → Harness seam → parity status*. This is the durable
    design-language reference for all future UI.
  - Verification: `cargo test -p harness --test config_docs_reference_test` (if
    the doc is referenced) or `scripts/test-lanes.sh fast`; doc renders.
  - Evidence:
- [x] **P1-2 · Doc corrections for retained scope · P1 · docs**
  - In `docs/roadmap-v1.md` / `docs/config.md`: state that per-model-family
    prompts are OpenCode parity (not orchestration overreach); that category
    routes and the extra native tools are retained; that no standalone `write`
    tool ships (hashline append creates files).
  - Verification: `scripts/test-lanes.sh fast` docs-reference tests green.
  - Evidence:

### Phase 2 — Theme role remap

- [~] **P2-1 · Remap theme roles onto OpenCode palette · P1 · UI (WS-A/V-A)**
  - Repoint primary/selection/section-header/accent/secondary roles to the
    OpenCode tokens; keep orange only where OpenCode uses warning-orange. Route
    any hardcoded `Color::Rgb` call sites through theme tokens.
  - Adaptation: record any role Harness keeps deliberately different.
  - Verification: `cargo test -p harness-tui` (reviewed snapshot updates with
    behavior-vs-fixture verdict); re-capture start + session screenshots.
  - Evidence: REVERTED 2026-06-16 — harness-tui source and snapshots rolled back to commit `3ffaf9a5`; redo against `inspirations/opencode/` while preserving Harness identity.

### Phase 3 — Transcript + tool presentation

- [~] **P3-1 · Unbox user messages · P1 · UI (WS-C/V-C)**
  - Render user turns as flowing text with a subtle marker, not a heavy box.
  - Verification: transcript render snapshot (reviewed); compare to chat ex. 1.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-2 · Slim prompt input + agent/model line above · P1 · UI (WS-C/V-C)**
  - Remove the embedded `Session:` line and tall padding; agent/model line moves
    above a slim input. Composer *behavior* unchanged (prior PRD owns it).
  - Verification: composer render snapshot; existing composer tests green.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-3 · Footer = agent·model·variant / tokens·cost·ctrl+p · P1 · UI (WS-C/V-C)**
  - Coordinate with prior PRD T-UI-01; this card sets the visual target. Status
    banner on its own line above the footer only when present.
  - Verification: `footer` render test at 100×30 and 60×20; compare to ex. 1/3.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-4 · Sidebar: generated title, LSP section, MCP status dots, diffstat, brand line · P1 · UI (WS-C/V-C)**
  - Title = generated session title (not `run_<id>`); verify/finish LSP section,
    per-server MCP status dots, Modified-Files `+N -M`; add `• Harness <version>`
    footer. Coordinate with prior PRD T-UI-08a.
  - Verification: sidebar render snapshot; compare to chat ex. 3/4 sidebar.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-5 · Inline `# Todos` + `Thinking:` blocks · P2 · UI (WS-C/V-C)**
  - Verify current todo/thinking placement; align to OpenCode's inline forms.
  - Verification: transcript snapshot with todos + thinking fixture.
  - Evidence: REVERTED 2026-06-16 — any implementation was rolled back; evaluate against `inspirations/opencode/` when transcript work resumes.
- [~] **P3-6 · Verb-form tool titles · P1 · UI (WS-D/T-A)**
  - Replace `generic_tool_name`'s raw-id passthrough with a verb map covering
    core + extra tools; MCP/unknown fall back to titlecased name.
  - Verification: `cargo test -p harness-tui` title tests (add per-tool cases).
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-7 · Inline diff for edit/apply_patch · P1 · UI (WS-D/T-B)**
  - Wire the existing inline-diff machinery to fire for apply_patch; raw payload
    becomes secondary/disclosure; artifact link kept as secondary.
  - Verification: render test for an apply_patch tool row showing a colored
    line-numbered diff; compare to chat ex. 4.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-8 · Fix markdown word-fusion / stray-highlight bug · P1 · UI (WS-D/T-C)**
  - Reproduce from `live_proxy` content; fix; add regression test.
  - Verification: `cargo test -p harness-tui` markdown render test.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P3-9 · Trim per-turn chrome bullets · P2 · UI (WS-D/T-D)**
  - Remove inline `• agent · model · Ns` lines; move timing to footer/sidebar or
    disclosure; align remaining marker glyphs to OpenCode.
  - Verification: transcript snapshot before/after; compare to chat ex. 1.
  - Evidence: REVERTED 2026-06-16 — any implementation was rolled back; evaluate against `inspirations/opencode/` when transcript work resumes.

### Phase 4 — Start screen + command palette

- [~] **P4-1 · Start screen composition 1:1 · P1 · UI (WS-B/V-B)**
  - Wordmark treatment, single in-box metadata line, bottom status bar, hint
    cluster, tip line.
  - Verification: startup render snapshot; compare to `Opencode start screen.png`.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P4-2 · Command palette styling 1:1 · P1 · UI (WS-E/V-D)**
  - Purple headers, salmon selection bar, keybinding right column, drop inline
    descriptions.
  - Verification: palette render snapshot (reviewed); compare to commands menu.
  - Evidence: REVERTED 2026-06-16 — any code changes were rolled back; redo after P2-1 against `inspirations/opencode/`.
- [~] **P4-3 · Fill command set · P2 · UI (WS-E/V-D)**
  - Add palette rows for commands Harness already has (model switch, skills,
    status, theme, help, docs, session new/continue/replay). No invented
    commands for absent features.
  - Verification: palette test asserts each row maps to a real action.
  - Evidence: REVERTED 2026-06-16 — any implementation was rolled back; evaluate against `inspirations/opencode/` when palette work resumes.

### Phase 5 — Backend features (parallelizable from Phase 1)

- [x] **P5-1 · Working-tree revert/undo · P1 · backend (WS-F)**
  - **Before the first edit (rule §0.2.4):** read
    `inspirations/opencode/packages/opencode/src/session/revert.ts` end to end and
    survey its neighbors (`snapshot`/snap track-restore, `session/*`,
    `cli/cmd/tui/util/revert-diff.ts`, `routes/session/index.tsx` undo/redo
    bindings); also skim Pi's snapshot/session approach under
    `inspirations/pi_agent_rust/src` and `inspirations/pi-mono/packages/coding-agent/src/core`
    for the snapshot-design choice. Do not design this from memory.
  - Snapshot the working tree before a message/tool batch and let the operator
    restore it. Coordinator-owned. Decide snapshot storage (git stash/ref or
    artifact store) — record the choice and rationale in the task. Surface an undo
    action in the TUI (intent only).
  - Invariants: restore is a **live** action; **no replay-time side effects**;
    snapshots redacted of secrets; no rewrite of `events.jsonl`.
  - Verification: `cargo test -p harness-core` revert tests incl. a
    **    replay-absence test** (replay never restores the tree); `coord_test`.
  - Adaptation: record where Harness semantics differ from OpenCode's git-based
    undo.
  - Evidence: Unaffected by 2026-06-16 visual revert; backend implementation remains preserved.
- [x] **P5-2 · Auto-format on edit · P1 · backend (WS-G)**
  - **Before the first edit (rule §0.2.4):** read
    `inspirations/opencode/packages/opencode/src/format/index.ts` and the
    `format/` directory (formatter registry, extension→formatter matching,
    enable/disable handling) end to end, and trace where OpenCode invokes it
    after a write. Do not design this from memory.
  - Run configured formatters after a successful write/edit, by file extension,
    behind a `tui.json`/`harness.json` toggle (decide which contract; default-on
    or default-off — record the choice). Coordinator-owned; format failures are
    non-fatal and surfaced, not silent.
  - Verification: `cargo test` formatter-dispatch test (mock formatter);
    disabled-path test; failure-is-non-fatal test.
  - Evidence: Unaffected by 2026-06-16 visual revert; backend implementation remains preserved.

### Phase 6 — Regression, evidence, dispositions

- [x] **P6-1 · Full deterministic regression · P1 · testing**
  - Verification: `scripts/test-lanes.sh all-deterministic` + `quality-gates`
    + clippy `-D warnings` green.
  - Evidence: Unaffected by 2026-06-16 visual revert; regression gate remains verified on the reverted tree.
- [~] **P6-2 · Side-by-side screenshot evidence · P1 · evidence**
  - Re-capture Harness start/session/palette/tool-diff at matching geometry and
    place beside the OpenCode references; record deltas as justified or fixed.
  - Verification: artifacts saved under the screenshots parity folder; deltas
    listed in §10.
  - Evidence: REVERTED 2026-06-16 — screenshots of the degraded UI cannot serve as parity evidence; recapture after P2/P3/P4 are redone.
- [x] **P6-3 · Disposition P2 cards · P1 · docs**
  - Mark each P2 box done (`- [x]`) or deferred (`- [~]`) with a written reason
    in §10.
  - Evidence: Updated 2026-06-16 — P2/P3-5/P3-9/P4-3 are deferred with written revert dispositions in §10; P3/P4/P6-2 visual work is also deferred.

---

## 9. Final acceptance criteria

- [~] **A1 · Theme.** Prompt border, selection bar, section headers, and accents
  use OpenCode's peach/salmon/purple/teal roles; warning-orange used only where
  OpenCode uses it. Verified by reviewed snapshots + start/session screenshots.
  **NOT ACHIEVED** — visual TUI changes were reverted on 2026-06-16 because they degraded the pre-PRD Harness TUI.
- [~] **A2 · Start screen** matches `Opencode start screen.png` composition
  (wordmark, single in-box metadata, bottom status bar, hint cluster, tip line),
  Harness-branded. Side-by-side recorded.
  **NOT ACHIEVED** — visual TUI changes were reverted on 2026-06-16 because they degraded the pre-PRD Harness TUI.
- [~] **A3 · Session transcript**: user messages unboxed; slim input with
  agent/model line above; footer = agent·model·variant / tokens·cost·ctrl+p;
  sidebar shows generated title + LSP + per-server MCP dots + diffstat + brand
  line; no inline per-turn chrome bullets. Side-by-side recorded.
  **NOT ACHIEVED** — visual TUI changes were reverted on 2026-06-16 because they degraded the pre-PRD Harness TUI.
- [~] **A4 · Tool calls**: verb-form titles; edit/apply_patch render an inline
  colored line-numbered diff (no raw-payload-as-primary); markdown word-fusion
  bug fixed. Compared to chat example 4.
  **NOT ACHIEVED** — visual TUI changes were reverted on 2026-06-16 because they degraded the pre-PRD Harness TUI.
- [~] **A5 · Command palette**: purple headers, salmon selection, keybinding
  column; rows map to real actions only. Compared to commands menu.
  **NOT ACHIEVED** — visual TUI changes were reverted on 2026-06-16 because they degraded the pre-PRD Harness TUI.
- [x] **A6 · Revert** works as a live operator action with a passing
  replay-absence test; no `events.jsonl` rewrite; secrets redacted.
- [x] **A7 · Auto-format** runs after edits behind a documented toggle; failures
  are non-fatal and surfaced; disabled path verified.
- [x] **A8 · Scope decisions honored**: categories, family prompts, and extra
  native tools retained; no `write` tool added; no custom markdown commands.
- [x] **A9 · Gates green**: `fast`, `quality-gates`, `all-deterministic`, clippy
  `-D warnings`; no test/snapshot weakened; every checked box has evidence.
- [x] **A10 · Docs**: `docs/opencode-tui-parity.md` exists; §3 corrections
  landed; changed contracts updated together with code.

---

## 10. Dispositions, deviations, and dogfooding evidence

> Append here: P2 dispositions (`- [~]` reasons), any **Adaptation:** notes where
> Harness deviates from OpenCode and why, screenshot-delta justifications, and
> the final dogfooding note (what was launched, what was observed). Do not edit
> earlier sections to hide a deviation — record it here.

### P2 dispositions

- **P3-5 · Inline `# Todos` + `Thinking:` blocks** — `REVERTED 2026-06-16`. Any visual changes were rolled back to commit `3ffaf9a5`; re-evaluate against `inspirations/opencode/` when transcript work resumes.
- **P3-9 · Trim per-turn chrome bullets** — `REVERTED 2026-06-16`. Any visual changes were rolled back to commit `3ffaf9a5`; redo when the transcript layout is re-attempted.
- **P4-3 · Fill command set** — `REVERTED 2026-06-16`. Any visual changes were rolled back to commit `3ffaf9a5`; redo command-set work against the restored palette in `inspirations/opencode/`.

### Adaptations (Harness wins)

- **Revert/undo**: OpenCode tracks git refs directly; Harness stores pre-edit workspace snapshots in the artifact store and applies inverse hashline edits. This is coordinator-owned, replay-safe, and writes no `events.jsonl` rewrite. Replay-absence test proves the tree is never restored during replay.
- **Auto-format**: Config lives in `harness.jsonc` under `formatter` (not `tui.json`), per the public runtime-config contract. Default is on; failures are non-fatal surfaced events.
- **Cost placeholder**: `REVERTED 2026-06-16` — this footer design was rolled back with the rest of the visual TUI; revisit when visual parity work resumes.
- **Generated title fallback**: `REVERTED 2026-06-16` — this sidebar design was rolled back with the rest of the visual TUI; revisit when visual parity work resumes.
- **Side-by-side evidence format**: `REVERTED 2026-06-16` — no screenshots were captured from the degraded UI; portable snapshots will be recaptured after the visual work is redone against `inspirations/opencode/`.

### Screenshot / snapshot deltas (status after 2026-06-16 revert)

| Surface | Delta vs. OpenCode reference | Resolution |
|---|---|---|
| Start screen | Wordmark, metadata line above composer, bottom status/hint cluster, tip line | Reverted 2026-06-16; redo required |
| Session transcript | User turns were boxed; now plain text with marker | Reverted 2026-06-16; redo required |
| Footer | Stacked multi-line footer; now single-line agent·model·variant / tokens·cost·ctrl+p | Reverted 2026-06-16; redo required |
| Sidebar | LSP details missing, no MCP dots, no diffstat, raw run id | Reverted 2026-06-16; redo required |
| Tool diff | `apply_patch` dumped raw `*** Begin Patch ***` payload | Reverted 2026-06-16; redo required |
| Palette | Gray/orange styling, no keybinding column | Reverted 2026-06-16; redo required |
| Markdown | Adjacent styled spans fused words | Reverted 2026-06-16; redo required |

### Final dogfooding evidence

- `scripts/test-lanes.sh fast` PASS — fmt/check/nextest_ci green.
- `scripts/test-lanes.sh quality-gates` PASS — static test-suite gates and forbidden-branding green.
- `scripts/test-lanes.sh all-deterministic` PASS — simulation, fast, integration, and PTY signoff lanes green.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` PASS.
- `cargo test -p harness-tui` 0 failures.
- `cargo test -p harness-core` 0 failures including revert and formatter tests.
- Changed files: `.gitignore`, `scripts/check-forbidden-branding.py`, `crates/harness-tui/src/ui_transcript_tests.rs`, plus all implementation work completed in prior turns.

### 2026-06-16 visual parity revert

On 2026-06-16 the visual TUI work for this PRD was reverted. User feedback was
that the applied changes made the Harness TUI look worse than both the pre-PRD
Harness TUI and the OpenCode reference, so every harness-tui source file and
snapshot was rolled back to commit `3ffaf9a5`. The backend features
(working-tree revert and auto-format) were not affected and remain preserved.
Each visual/ui task must be redone using the actual OpenCode source under
`inspirations/opencode/` as the reference while preserving the Harness TUI's
visual identity. The P2 dispositions, Adaptations, and screenshot-delta rows
above that claim visual work was completed are withdrawn; task boxes and
acceptance criteria in this PRD have been updated to reflect the revert.

---

## 11. Instructions for the implementation agent

1. Start at Phase 1; it is cheap and makes later claims honest. Phase 5 (backend
   features) may run in parallel if staffed.
2. For **every** card (visual, tool, backend, docs): re-open the cited
   `inspirations/` reference file(s) and survey their neighbors before the first
   edit (rule §0.2.4) — never work from memory. For visual/tool cards, also open
   the named screenshot, implement, re-capture at matching geometry, and compare
   before checking the box with evidence (§0.3).
3. Prefer the narrowest test lane that proves the change. Reviewed snapshot
   updates require a behavior-vs-fixture verdict in the commit message.
4. You own the *how*. Named files are the likely seam; if a cleaner one exists,
   use it and note it on the Evidence line. Surface any assumption that, if
   wrong, would change the approach — do not silently guess.
5. When the reference conflicts with a §0.4 invariant, Harness wins; record the
   **Adaptation:**. When this PRD's description disagrees with the reference,
   re-read the reference and record which was wrong — do not invent a third
   behavior.
6. Keep coordinator authority and replay-safety intact, especially for revert.
